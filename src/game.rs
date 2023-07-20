#![allow(dead_code)]
//pub struct SimpleMonster{
//    id: std::ops::Range { start: 3, end: 5 },
//}
use std::slice::Iter;
use std::iter::Copied;
use rand::thread_rng;
use rand::seq::SliceRandom;
use arrayvec::ArrayVec;
use serde::{Serialize, Deserialize};

use crate::details::create_enum_iter;


create_enum_iter!{
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Rank {
        //Ace    aces not exist in this game
        Two   = 1,
        Three = 2,
        Four  = 3,
        Five  = 4,
        Six   = 5,
        Seven = 6,
        Eight = 7,
        Nine  = 8,
        Ten   = 9,
        Jack  = 10,
        Queen = 11,
        King  = 12,
    }
}

create_enum_iter!{
    #[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
    pub enum Suit {
        Hearts = 0,
        Diamonds =1,
        Clubs =2,
        Spades=3,
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize, Deserialize)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit
}

#[derive(Debug)]
pub struct Deck {
    cards : ArrayVec::<Card, {Self::DECK_SIZE}>,
}
impl Deck {
    pub const DECK_SIZE: usize = Rank::all().len() * Suit::all().len(); //48
    pub fn shuffle(&mut self) -> &mut Self {
        self.cards.shuffle(&mut thread_rng());
        self
    }
    pub fn empty() -> Self {
        Deck{ cards: Default::default() }
    }
}
impl Default for Deck {
    fn default() -> Self {
        Deck { 
            cards: Rank::iter()
            .flat_map(|r| {
                    Suit::iter().map(move |s| Card{suit: s, rank: r})
            }).collect()
        }
    }
}

use crate::protocol::Role;
use crate::details::impl_from;

impl_from!{( )  Role    => Suit,
                Warrior => Hearts,
                Rogue   => Diamonds,
                Paladin => Clubs,
                Mage    => Spades,
        }


struct PlayerDeck {
    ranks:  ArrayVec<Rank, {Rank::all().len()}>,
    suit:   Suit
}

impl PlayerDeck {
    fn new(suit: Suit) -> Self {
        PlayerDeck{suit, ranks: (*Rank::all()).into()}
    }
    fn shuffle(&mut self) -> & mut Self{
        self.ranks.shuffle(&mut thread_rng());
        self
    }
    fn empty(suit: Suit) -> Self {
        PlayerDeck{suit, ranks: Default::default() }
    }
}


// all 48 monsters 
// bosses: 4 kings, 4 queens, 4 knaves
// 36 simple monsters
pub trait MonsterDeck {
    fn new_monster_deck() -> Deck;
}

impl MonsterDeck for Deck {
    fn new_monster_deck() -> Deck {
        let mut rng = thread_rng();
        let mut bosses = [Rank::Jack, Rank::King, Rank::Queen ]
                .map(|c| Suit::all().map( |suit|  Card{ suit, rank: c } ));
        bosses.iter_mut().for_each(|b| b.shuffle(&mut rng));

        let mut card_iter = Rank::all()[..Rank::Ten as usize]
            .iter()
            .flat_map(|r| {
                Suit::iter().map(|s| Card{suit: s, rank: *r})
        });
        let mut other_cards : [Card; Rank::Ten as usize * Suit::all().len()] 
                = core::array::from_fn(|_| {
                card_iter.next().unwrap()
        });
        other_cards.shuffle(&mut rng);
        let mut other_cards_iter = other_cards.iter();
        Deck{ cards :  core::array::from_fn(|i| {
            let i = i + 1; // start from 1, not 0
            if  i % 4 == 0 { // each 4 card is a boss
                // it's time to a boss !!
                // select from 0..2 with i/4 % 3 type of bosses,
                // and i/4 % 4 index 0..3 in the boss array 
                // 4, 16, 28, 30  cards should be a king  4/4%3 =1; 16/4%3=1..
                // 8, 20, 32, 44  queen  8/4 % 3 = 2; 20/4 % 3=2 ..
                // 12, 24, 36, 38 jack  12/4%3=0 ..
                // ------
                // i/4 % 4 select 0..3 -> 4/4 % 4=1, 16/4%4 = 0.. 
                bosses[i/4 % 3 as usize][i/4 % 4 as usize]
            } else {
                *other_cards_iter.next()
                    .expect("count of numeric cards must be 
                       a cound of all deck minus a count of court(face) cards")
            }
        }).into() }
    }
}



#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn create_deck() {
        let deck = Deck::new_monster_deck();
        deck.cards.iter().enumerate().for_each(|(i, m)| println!("{i}: {:?}", m));
        //println!("{:?}", deck);
        //let result = 2 + 2;
        //assert_eq!(result, 4);
    }
}
